// Spotify 플레이어 위젯: 미디어 위젯(`media.player`)의 대상이 Spotify이고 `Spotify theme`을 켰을 때 `MediaWidget` 대신 쓴다(`shell/WidgetSlot`).
// 사용자 요청으로 덱의 하드웨어풍 테마(위젯 틀, 나사, 디스플레이, 돌출 버튼)를 따르지 않고 Spotify 앱의 재생 화면처럼 그린다:
//   카드         둥근 모서리의 어두운 카드. 위쪽은 앨범 아트의 대표 색(`host.extra.color`)에서 아래쪽 Spotify 검정(#121212)으로 이어지는 그라데이션
//   왼쪽         앨범 아트(살짝 둥근 정사각형)
//   오른쪽       곡 제목(굵은 흰 글씨, 길면 흐른다), 아티스트(회색), 대기열의 다음 곡, 아래쪽에 재생 기기(초록, 누르면 기기 창)
//   진행 줄      경과 시간 / 가는 막대(흰 채움과 둥근 손잡이) / 전체 시간
//   제어 줄      좋아요 / 셔플 / 이전 곡 / 재생·일시정지(흰 원) / 다음 곡 / 반복 / 대기열. 버튼 틀 없이 아이콘만 두고,
//                켜진 셔플·반복과 좋아요는 Spotify 초록으로 칠하며 셔플·반복 아래에 작은 점을 찍는다
// 기능은 `MediaWidget`의 Spotify 모드와 같다(같은 액션을 보낸다. 대기열·기기 창은 `shell/SpotifySheet`). 재생 위치 계산도 같다.
// 연결되지 않았으면 사유(`host.offline`)를, 재생 중인 곡이 없으면 안내 문장을 보여 주고 제어 아이콘을 흐리게 한다.
import QtQuick
import "../kit"

WidgetBase {
    id: root

    readonly property var media: host && host.media ? host.media : null
    readonly property var sp: host && host.extra ? host.extra : null
    readonly property bool live: !!host && !host.offline
    readonly property bool canControl: live && !!host.caps && host.caps.indexOf("media.control") >= 0
    readonly property bool playing: !!media && media.status === "playing"
    readonly property int len: media ? Math.floor(media.length_ms / 1000) : 0
    readonly property bool hasLoop: !!media && media.loop_mode !== undefined && media.loop_mode !== null
    readonly property bool hasShuffle: !!media && media.shuffle !== undefined && media.shuffle !== null
    readonly property var art: host && host.art && media && media.art && host.art.id === media.art ? host.art : null

    readonly property color green: Theme.spotifyGreen
    readonly property color black: "#121212"
    readonly property color grey: "#b3b3b3"
    readonly property color topColor: sp && sp.color && media ? Theme.mix(Qt.color(black), Qt.color(sp.color), 0.62) : "#2a2a2a"

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
        running: root.playing
        onTriggered: root.nowAt = Date.now()
    }
    readonly property real posMs: {
        if (!media)
            return 0
        const p = basePos + (playing ? Math.max(0, nowAt - baseAt) : 0)
        return media.length_ms > 0 ? Math.min(p, media.length_ms) : p
    }
    function mmss(s) { return Math.floor(s / 60) + ":" + String(s % 60).padStart(2, "0") }

    component Icon: Item {
        id: icon
        property string glyph: ""
        property color tint: "white"
        property real size: 28
        property bool usable: true
        property bool dot: false
        signal tapped()
        width: 64
        height: 64
        opacity: usable ? 1 : 0.35
        Glyph {
            anchors.centerIn: parent
            name: icon.glyph
            size: icon.size
            color: icon.tint
            scale: hit.pressed ? 0.88 : 1
            Behavior on scale { NumberAnimation { duration: 80 } }
        }
        Rectangle {
            visible: icon.dot
            anchors { horizontalCenter: parent.horizontalCenter; top: parent.verticalCenter; topMargin: icon.size / 2 + 6 }
            width: 5
            height: 5
            radius: 2.5
            color: root.green
        }
        MouseArea {
            id: hit
            anchors.fill: parent
            enabled: icon.usable
            onClicked: icon.tapped()
        }
    }

    Rectangle {
        id: card
        anchors.fill: parent
        radius: 12
        gradient: Gradient {
            GradientStop { position: 0; color: root.topColor }
            GradientStop { position: 0.85; color: root.black }
        }
    }

    Item {
        id: body
        anchors { fill: parent; margins: 20 }

        Rectangle {
            id: artBox
            width: Math.min(body.height - controls.height - progress.height - 22, Math.round(body.width * 0.34))
            height: width
            radius: 6
            color: "#282828"
            Glyph {
                anchors.centerIn: parent
                name: "play"
                size: Math.round(artBox.width * 0.32)
                color: "#535353"
                visible: cover.status !== Image.Ready
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

        Item {
            id: info
            anchors { left: artBox.right; leftMargin: 20; right: parent.right; top: artBox.top; bottom: artBox.bottom }
            Marquee {
                id: title
                anchors { left: parent.left; right: parent.right; top: parent.top; topMargin: 6 }
                height: 40
                text: !root.live ? "Spotify" : !root.media ? "Nothing playing" : (root.media.title || "Unknown title")
                color: "white"
                font.family: Theme.font
                font.pixelSize: 30
                font.bold: true
                running: root.playing
            }
            Txt {
                id: artist
                anchors { left: parent.left; right: parent.right; top: title.bottom; topMargin: 2 }
                text: !root.live ? (root.host ? root.host.offline : "")
                    : !root.media ? "Play something on a phone, PC or speaker"
                    : [root.media.artist, root.media.album].filter((s) => !!s).join(" · ")
                color: root.grey
                font.pixelSize: 19
                elide: Text.ElideRight
                wrapMode: root.live ? Text.NoWrap : Text.WordWrap
                maximumLineCount: 2
            }
            Txt {
                anchors { left: parent.left; right: parent.right; top: artist.bottom; topMargin: 14 }
                visible: !!root.sp && !!root.media && root.sp.queue.length > 0
                text: visible ? "Next: " + root.sp.queue[0].title + (root.sp.queue[0].artist ? " · " + root.sp.queue[0].artist : "") : ""
                color: "#8a8a8a"
                font.pixelSize: 16
                elide: Text.ElideRight
            }
            Item {
                anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
                height: 34
                visible: root.live
                Row {
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: 8
                    scale: deviceHit.pressed ? 0.96 : 1
                    Glyph { anchors.verticalCenter: parent.verticalCenter; name: "device"; size: 18; color: root.sp && root.sp.device ? root.green : root.grey }
                    Txt {
                        anchors.verticalCenter: parent.verticalCenter
                        text: root.sp && root.sp.device ? "Playing on " + root.sp.device : "Choose a device"
                        color: root.sp && root.sp.device ? root.green : root.grey
                        font.pixelSize: 16
                        font.bold: true
                    }
                }
                MouseArea {
                    id: deviceHit
                    anchors.fill: parent
                    onClicked: root.action("spotify.devices", { themed: true })
                }
            }
        }

        Item {
            id: progress
            anchors { left: parent.left; right: parent.right; bottom: controls.top; bottomMargin: 4 }
            height: 26
            opacity: root.media ? 1 : 0.35
            Txt {
                id: elapsed
                anchors { left: parent.left; verticalCenter: parent.verticalCenter }
                width: 52
                text: root.media ? root.mmss(Math.floor(root.posMs / 1000)) : "-:--"
                color: root.grey
                font.pixelSize: 14
            }
            Txt {
                id: total
                anchors { right: parent.right; verticalCenter: parent.verticalCenter }
                width: 52
                horizontalAlignment: Text.AlignRight
                text: root.len > 0 ? root.mmss(root.len) : "-:--"
                color: root.grey
                font.pixelSize: 14
            }
            Rectangle {
                id: track
                anchors { left: elapsed.right; right: total.left; verticalCenter: parent.verticalCenter; leftMargin: 6; rightMargin: 6 }
                height: 4
                radius: 2
                color: "#4d4d4d"
                readonly property real ratio: root.media && root.media.length_ms > 0 ? Math.min(1, root.posMs / root.media.length_ms) : 0
                Rectangle {
                    width: parent.width * track.ratio
                    height: parent.height
                    radius: 2
                    color: "white"
                }
                Rectangle {
                    visible: !!root.media
                    x: parent.width * track.ratio - width / 2
                    anchors.verticalCenter: parent.verticalCenter
                    width: 12
                    height: 12
                    radius: 6
                    color: "white"
                }
            }
        }

        Item {
            id: controls
            anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
            height: 72

            Icon {
                anchors { left: parent.left; verticalCenter: parent.verticalCenter }
                glyph: root.sp && root.sp.liked === true ? "hearton" : "heart"
                tint: root.sp && root.sp.liked === true ? root.green : root.grey
                usable: root.canControl && !!root.media && !!root.sp && root.sp.liked !== null && root.sp.liked !== undefined
                onTapped: root.action("spotify.like", { on: !(root.sp.liked === true) })
            }
            Row {
                anchors.centerIn: parent
                spacing: 18
                Icon {
                    anchors.verticalCenter: parent.verticalCenter
                    glyph: "shuffle"
                    size: 26
                    readonly property bool on: root.hasShuffle && root.media.shuffle
                    tint: on ? root.green : root.grey
                    dot: on
                    usable: root.canControl && root.hasShuffle
                    onTapped: root.action("media.shuffle", { on: !on })
                }
                Icon {
                    anchors.verticalCenter: parent.verticalCenter
                    glyph: "prev"
                    usable: root.canControl && !!root.media && root.media.can_prev
                    onTapped: root.action("media.prev", null)
                }
                Item {
                    anchors.verticalCenter: parent.verticalCenter
                    width: 72
                    height: 72
                    opacity: root.canControl && !!root.media ? 1 : 0.35
                    Rectangle {
                        anchors.centerIn: parent
                        width: 64
                        height: 64
                        radius: 32
                        color: "white"
                        scale: playHit.pressed ? 0.92 : 1
                        Behavior on scale { NumberAnimation { duration: 80 } }
                        Glyph {
                            anchors.centerIn: parent
                            anchors.horizontalCenterOffset: root.playing ? 0 : 2
                            name: root.playing ? "pause" : "play"
                            size: 30
                            color: root.black
                        }
                    }
                    MouseArea {
                        id: playHit
                        anchors.fill: parent
                        enabled: root.canControl && !!root.media
                        onClicked: root.action("media.play_pause", null)
                    }
                }
                Icon {
                    anchors.verticalCenter: parent.verticalCenter
                    glyph: "next"
                    usable: root.canControl && !!root.media && root.media.can_next
                    onTapped: root.action("media.next", null)
                }
                Icon {
                    anchors.verticalCenter: parent.verticalCenter
                    readonly property string mode: root.hasLoop ? root.media.loop_mode : "none"
                    glyph: mode === "track" ? "repeat1" : "repeat"
                    size: 26
                    tint: mode !== "none" ? root.green : root.grey
                    dot: mode !== "none"
                    usable: root.canControl && root.hasLoop
                    onTapped: root.action("media.loop", { mode: mode === "none" ? "playlist" : mode === "playlist" ? "track" : "none" })
                }
            }
            Icon {
                anchors { right: parent.right; verticalCenter: parent.verticalCenter }
                glyph: "queue"
                tint: root.grey
                usable: root.live
                onTapped: root.action("spotify.queue", { themed: true })
            }
        }
    }
}
