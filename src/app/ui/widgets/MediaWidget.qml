// 미디어 플레이어 위젯(복합 위젯): 디스플레이에 앨범 아트와 트랙 정보, 진행 표시줄을 보여 주고 하단에 제어 키 7개를 배치한다:
//   반복 / 이전 곡 / 5초 뒤로 / 재생·일시정지 / 5초 앞으로 / 다음 곡 / 셔플
// 가로가 넓은 형태(`4x2`) 기준으로 레이아웃을 설계했다: 앨범 아트가 디스플레이 높이를 채우고 텍스트와 진행 막대가 오른쪽에 배치된다.
// 좁게 배치되면 앨범 아트가 너비에 맞춰 축소된다. 하단 키는 1줄로 위젯 너비를 채운다(재생 키만 약간 넓음).
//
// 표시 내용은 연결된 호스트(PC)가 보낸 재생 정보(`host.media`)다. 버튼을 누르면 호스트에 액션을 전송한다
// (`media.prev` / `media.play_pause` / `media.next` / `media.seek` / `media.loop` / `media.shuffle`).
// UI는 호스트가 새 상태를 내려 줄 때만 바뀌며 자체적으로 상태를 토글하지 않는다(반복/셔플 상태 포함).
//   반복        탭할 때마다 none -> playlist -> track -> none 순환. 켜지면 눌린 상태를 유지하며 한 곡 반복 시 아이콘에 "1"이 들어간다.
//   셔플        탭할 때마다 ON/OFF 전환. 켜지면 눌린 상태를 유지한다.
//   (반복과 셔플의 활성 상태는 눌린 버튼 형태로만 구분하고, 5초 탐색 키에는 숫자를 표시하지 않는다 — 2026-11-17 사용자 확정)
//   플레이어가 지원하지 않는 기능(`media.loop_mode` / `media.shuffle === null`, `media.can_seek === false`)은 비활성화된다.
//   호스트 미연결 시      `"No host"` 표시 및 모든 버튼 비활성화
//   활성 플레이어 없을 시 `"Nothing playing"` 표시
// 호스트가 허용한 권한(`host.caps`)에 따라 동작이 제한된다(`docs/04-host-link.md` 4.5절):
//   `media.info` 없음     트랙 정보가 오지 않으므로 `"Not shared"` 표시
//   `media.control` 없음  제어 키가 비활성화됨(전송해도 호스트가 거절)
// 재생 위치 계산: 호스트는 트랙 변경/재생·일시정지/탐색 등 이벤트 시점에만 상태를 보내며 매초 보내지 않는다.
//   따라서 수신된 위치에서 재생 중인 동안 클라이언트 측에서 `media.rate` 배속으로 경과 시간을 누적 계산한다(0이면 정지).
//   새 상태가 오면 서버 기준 위치로 재동기화된다(데몬이 넘겨주는 `position_ms`는 전송 시점 기준으로 보정된 값).
// 대상이 Spotify(`"@spotify"`)면 `host`는 덱이 직접 조회하는 Spotify 계정이다(`shell/DaemonLink`의 `spotify`). 액션도 덱이 Spotify로 처리한다.
//   연결되지 않았거나 조회에 실패하면 `host.offline`에 사유가 담기며, 이때는 그 사유를 표시하고 버튼을 비활성화한다.
// 대상이 Spotify면 Spotify 앱처럼 동작한다(`host.extra`는 `shell/DaemonLink`의 `spotify` 참고):
//   키 배치       좋아요 / 셔플 / 이전 곡 / 재생·일시정지 / 다음 곡 / 반복 / 대기열(5초 탐색 키 대신 좋아요와 대기열)
//   정보 줄       부제 아래에 대기열의 다음 곡, 진행 막대 위에 재생 기기 이름. 기기 줄을 누르면 기기 목록 창이 뜬다(`shell/SpotifySheet`)
// `Spotify theme`을 켜면 이 위젯 대신 `widgets/SpotifyPlayer`가 쓰인다(`shell/WidgetSlot`).
// 앨범 아트: `host.art` (`id`, `mime`, `data`). 현재 트랙의 이미지(`media.art === host.art.id`)일 때만 표시한다.
//   이미지는 트랙 정보보다 늦게 도착하므로 수신 전이나 커버가 없는 곡에서는 재생/일시정지 아이콘을 대신 표시한다.
import QtQuick
import "../kit"

CompositeWidget {
    id: root

    readonly property var media: host && host.media ? host.media : null
    readonly property var caps: host && host.caps ? host.caps : []
    readonly property bool canSee: caps.indexOf("media.info") >= 0
    readonly property bool canControl: caps.indexOf("media.control") >= 0
    readonly property bool playing: !!media && media.status === "playing"
    readonly property real rate: media && media.rate !== undefined ? media.rate : 1
    readonly property int len: media ? Math.floor(media.length_ms / 1000) : 0

    property real basePos: 0
    property double baseAt: 0
    property double nowAt: 0
    onMediaChanged: {
        basePos = media ? media.position_ms : 0
        baseAt = Date.now()
        nowAt = baseAt
    }
    Timer {
        interval: 250
        repeat: true
        running: root.playing && root.rate > 0
        onTriggered: root.nowAt = Date.now()
    }
    readonly property real posMs: {
        if (!media)
            return 0
        const p = basePos + (playing ? Math.max(0, nowAt - baseAt) * rate : 0)
        return media.length_ms > 0 ? Math.min(p, media.length_ms) : p
    }
    readonly property int pos: Math.floor(posMs / 1000)

    readonly property var art: host && host.art && media && media.art && host.art.id === media.art ? host.art : null

    function mmss(s) { return Math.floor(s / 60) + ":" + String(s % 60).padStart(2, "0") }

    corner: Indicator { on: root.playing }

    Display {
        id: disp
        anchors { left: parent.left; right: parent.right; top: parent.top; bottom: keys.top; bottomMargin: 12 }

        Rectangle {
            id: artBox
            x: 14
            y: 14
            width: Math.min(parent.height - 28, Math.round(parent.width * 0.3))
            height: width
            radius: Theme.rDetail
            color: Theme.displayOff
            Glyph {
                anchors.centerIn: parent
                name: root.playing ? "play" : "pause"
                size: Math.round(Math.min(96, artBox.width * 0.4))
                color: Theme.displayDim
                visible: !!root.media && cover.status !== Image.Ready
            }
            Image {
                id: cover
                anchors.fill: parent
                source: root.art ? "data:" + root.art.mime + ";base64," + root.art.data : ""
                visible: status === Image.Ready
                fillMode: Image.PreserveAspectCrop
                sourceSize: Qt.size(width, height)
                asynchronous: true
                smooth: true
            }
        }
        Marquee {
            id: title
            anchors { left: artBox.right; leftMargin: 14; right: parent.right; rightMargin: 14; top: parent.top; topMargin: 22 }
            text: !root.host ? "No host" : root.host.offline ? root.host.name : !root.canSee ? "Not shared" : !root.media ? "Nothing playing" : (root.media.title || "Unknown title")
            color: root.media ? Theme.displayInk : Theme.displayDim
            font.pixelSize: Theme.fTitle
            font.bold: true
            running: root.playing
        }
        Txt {
            id: subtitle
            anchors { left: title.left; right: title.right; top: title.bottom; topMargin: 4 }
            text: !root.host ? "Run toss-connector on a PC"
                : root.host.offline ? root.host.offline
                : !root.media ? root.host.name
                : [root.media.artist, root.media.album].filter((s) => !!s).join(" · ")
            color: Theme.displayDim
            font.pixelSize: Theme.fSmall + 1
            elide: Text.ElideRight
        }

        Txt {
            anchors { left: title.left; right: title.right; top: subtitle.bottom; topMargin: 10 }
            visible: !!root.sp && !!root.media && root.sp.queue.length > 0
            text: visible ? "Next  " + root.sp.queue[0].title + (root.sp.queue[0].artist ? " · " + root.sp.queue[0].artist : "") : ""
            color: Theme.displayDim
            font.pixelSize: Theme.fSmall
            elide: Text.ElideRight
        }
        Item {
            anchors { left: title.left; right: title.right; bottom: elapsed.top; bottomMargin: 10 }
            height: 26
            visible: !!root.sp && root.host.connected
            Row {
                anchors.verticalCenter: parent.verticalCenter
                spacing: 8
                Glyph { anchors.verticalCenter: parent.verticalCenter; name: "device"; size: 18; color: Theme.displayDim }
                Txt {
                    anchors.verticalCenter: parent.verticalCenter
                    text: root.sp && root.sp.device ? root.sp.device : "Choose a device"
                    color: Theme.displayDim
                    font.pixelSize: Theme.fSmall
                }
            }
            MouseArea {
                anchors.fill: parent
                onClicked: root.action("spotify.devices", null)
            }
        }
        Txt {
            id: elapsed
            anchors { left: title.left; bottom: parent.bottom; bottomMargin: 14 }
            text: root.media ? root.mmss(root.pos) : "-:--"
            color: root.media ? Theme.displayInk : Theme.displayDim
            font.family: Theme.mono
            font.pixelSize: Theme.fSmall
        }
        Txt {
            id: total
            anchors { right: parent.right; rightMargin: 14; baseline: elapsed.baseline }
            text: root.len > 0 ? root.mmss(root.len) : "-:--"
            color: Theme.displayDim
            font.family: Theme.mono
            font.pixelSize: Theme.fSmall
        }
        ProgressBar {
            anchors { left: elapsed.right; leftMargin: 12; right: total.left; rightMargin: 12; verticalCenter: elapsed.verticalCenter }

            value: root.media && root.media.length_ms > 0 ? Math.min(1, root.posMs / root.media.length_ms) : 0
        }
    }

    readonly property int step: 5000
    readonly property var sp: host && host.id === "@spotify" && host.extra ? host.extra : null
    readonly property bool canSeek: canControl && !!media && !!media.can_seek
    readonly property bool hasLoop: !!media && media.loop_mode !== undefined && media.loop_mode !== null
    readonly property bool hasShuffle: !!media && media.shuffle !== undefined && media.shuffle !== null

    Row {
        id: keys
        anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
        height: 64
        spacing: 12
        readonly property real playWidth: 136
        readonly property real keyWidth: (width - 6 * spacing - playWidth) / 6

        component Key: Button {
            property bool usable: true
            width: keys.keyWidth
            height: keys.height
            enabled: usable
            opacity: usable ? 1 : Theme.dimmed
        }
        component StateKey: Key {
            id: stateKey
            property bool on: false
            property string glyph: ""
            held: on
            Glyph { anchors.centerIn: parent; name: stateKey.glyph; size: 28; color: stateKey.inkColor }
        }

        component LoopKey: StateKey {
            readonly property string mode: root.hasLoop ? root.media.loop_mode : "none"
            usable: root.canControl && root.hasLoop
            on: mode !== "none"
            onTapped: root.action("media.loop", { mode: mode === "none" ? "playlist" : mode === "playlist" ? "track" : "none" })
            glyph: mode === "track" ? "repeat1" : "repeat"
        }
        component ShuffleKey: StateKey {
            usable: root.canControl && root.hasShuffle
            on: root.hasShuffle && root.media.shuffle
            onTapped: root.action("media.shuffle", { on: !on })
            glyph: "shuffle"
        }

        StateKey {
            visible: !!root.sp
            usable: root.canControl && !!root.media && root.sp.liked !== null && root.sp.liked !== undefined
            on: root.sp ? root.sp.liked === true : false
            onTapped: root.action("spotify.like", { on: !on })
            glyph: on ? "hearton" : "heart"
        }
        ShuffleKey { visible: !!root.sp }
        LoopKey { visible: !root.sp }
        Key {
            id: prev
            usable: root.canControl && !!root.media && root.media.can_prev
            onTapped: root.action("media.prev", null)
            Glyph { anchors.centerIn: parent; name: "prev"; size: 28; color: prev.inkColor }
        }
        Key {
            id: back
            visible: !root.sp
            usable: root.canSeek
            onTapped: root.action("media.seek", { offset_ms: -root.step })
            Glyph { anchors.centerIn: parent; name: "rew"; size: 28; color: back.inkColor }
        }
        Key {
            id: play
            width: keys.playWidth
            color: Theme.buttonColor("orange")
            usable: root.canControl && !!root.media
            onTapped: root.action("media.play_pause", null)
            Glyph { anchors.centerIn: parent; name: root.playing ? "pause" : "play"; size: 30; color: play.inkColor }
        }
        Key {
            id: forward
            visible: !root.sp
            usable: root.canSeek
            onTapped: root.action("media.seek", { offset_ms: root.step })
            Glyph { anchors.centerIn: parent; name: "ffwd"; size: 28; color: forward.inkColor }
        }
        Key {
            id: next
            usable: root.canControl && !!root.media && root.media.can_next
            onTapped: root.action("media.next", null)
            Glyph { anchors.centerIn: parent; name: "next"; size: 28; color: next.inkColor }
        }
        LoopKey { visible: !!root.sp }
        ShuffleKey { visible: !root.sp }
        Key {
            id: queueKey
            visible: !!root.sp
            usable: !!root.host && root.host.connected
            onTapped: root.action("spotify.queue", null)
            Glyph { anchors.centerIn: parent; name: "queue"; size: 28; color: queueKey.inkColor }
        }
    }
}
