// Spotify 창: Spotify 미디어 위젯에서 연 대기열(`queue`) 또는 재생 기기(`devices`) 목록을 화면 가운데에 띄운다.
// 어느 창을 띄울지는 `link.spotifyView`가 정한다(`shell/DaemonLink`). 위젯이 요청을 보낼 때 열리고, 덱이 받아 온 목록이 오면 바로 갱신된다.
//   queue    지금 곡 다음에 나올 곡들(읽기만 한다)
//   devices  Spotify Connect 기기. 누르면 그 기기로 재생을 옮기고(`spotify.transfer`) 창을 닫는다
// 바깥 영역이나 닫기 버튼을 누르면 닫힌다.
//
// 모양은 창을 연 위젯을 따른다(`link.spotifyThemed`):
//   덱 테마      `MediaWidget`에서 열었을 때. 설정 화면과 같은 부품(라벨, `SettingsRow`, 인디케이터, `CLOSE` 버튼)
//   Spotify 테마 `widgets/SpotifyPlayer`에서 열었을 때. Spotify 앱의 목록처럼 어두운 둥근 카드(#282828)에 흰 굵은 제목,
//                대기열은 "Now playing"(지금 곡, 초록)과 "Next in queue" 묶음, 기기는 기기 아이콘과 이름(지금 기기는 초록과 "Listening on"),
//                닫기는 흰 둥근 버튼
import QtQuick
import "../kit"

Dialog {
    id: root
    property var link: null
    readonly property string view: link && link.connected ? link.spotifyView : ""
    readonly property bool themed: !!link && link.spotifyThemed
    readonly property var extra: link && link.spotify && link.spotify.extra ? link.spotify.extra : null
    readonly property var playing: link && link.spotify ? link.spotify.media : null
    readonly property var rows: !extra ? [] : view === "queue" ? extra.queue : view === "devices" ? extra.devices : []
    readonly property string emptyText: view === "queue" ? "Nothing queued." : "No device found. Open Spotify on a phone, PC or speaker first."

    readonly property color green: Theme.spotifyGreen
    readonly property color grey: "#b3b3b3"

    function close() { link.spotifyView = "" }
    function pick(row) {
        if (view !== "devices" || row.active)
            return
        link.action("spotify.transfer", { id: row.id }, "@spotify", 0)
        close()
    }

    visible: view !== ""
    shade: themed ? Qt.rgba(0, 0, 0, 0.6) : Qt.alpha(Theme.plateShade, 0.82)
    onDismissed: close()

    Dialog.Card {
        anchors.centerIn: parent
        width: parent.width - 2 * Theme.gridMargin - 80
        height: (root.themed ? spotifyBody.height : body.height) + 44
        radius: root.themed ? 14 : Theme.rFrame
        color: root.themed ? "#282828" : Theme.plate
        border.width: root.themed ? 0 : 1

        Column {
            id: body
            visible: !root.themed
            x: 22
            y: 22
            width: parent.width - 44
            spacing: 14
            Label { text: root.view === "queue" ? "UP NEXT" : "PLAY ON" }
            Txt {
                visible: root.rows.length === 0
                width: parent.width
                text: root.emptyText
                wrapMode: Text.WordWrap
                color: Theme.inkDim
                font.pixelSize: Theme.fBody
            }
            Flickable {
                visible: root.rows.length > 0
                width: parent.width
                height: Math.min(list.height, root.height * 0.55)
                contentHeight: list.height
                clip: true
                Column {
                    id: list
                    width: parent.width
                    Repeater {
                        model: root.themed ? [] : root.rows
                        SettingsRow {
                            required property var modelData
                            required property int index
                            width: list.width
                            title: modelData.title !== undefined ? modelData.title : modelData.name
                            note: modelData.artist !== undefined ? modelData.artist : modelData.kind
                            divider: index < root.rows.length - 1
                            onTapped: root.pick(modelData)
                            Indicator {
                                anchors.verticalCenter: parent.verticalCenter
                                visible: root.view === "devices"
                                on: modelData.active === true
                            }
                        }
                    }
                }
            }
            Row {
                anchors.right: parent.right
                Button {
                    width: 170
                    height: 60
                    text: "CLOSE"
                    onTapped: root.close()
                }
            }
        }

        Column {
            id: spotifyBody
            visible: root.themed
            x: 26
            y: 24
            width: parent.width - 52
            spacing: 10

            Txt {
                text: root.view === "queue" ? "Queue" : "Connect to a device"
                color: "white"
                font.pixelSize: 28
                font.bold: true
                bottomPadding: 6
            }

            Txt {
                visible: root.view === "queue" && !!root.playing
                text: "Now playing"
                color: "white"
                font.pixelSize: 18
                font.bold: true
            }
            Item {
                visible: root.view === "queue" && !!root.playing
                width: parent.width
                height: 60
                Column {
                    anchors.verticalCenter: parent.verticalCenter
                    width: parent.width
                    spacing: 2
                    Txt {
                        width: parent.width
                        text: root.playing ? root.playing.title : ""
                        color: root.green
                        font.pixelSize: 20
                        font.bold: true
                        elide: Text.ElideRight
                    }
                    Txt {
                        width: parent.width
                        text: root.playing ? root.playing.artist : ""
                        color: root.grey
                        font.pixelSize: 16
                        elide: Text.ElideRight
                    }
                }
            }
            Txt {
                visible: root.view === "queue" && root.rows.length > 0
                text: "Next in queue"
                color: "white"
                font.pixelSize: 18
                font.bold: true
                topPadding: 6
            }
            Txt {
                visible: root.rows.length === 0
                width: parent.width
                text: root.emptyText
                wrapMode: Text.WordWrap
                color: root.grey
                font.pixelSize: 18
            }
            Flickable {
                visible: root.rows.length > 0
                width: parent.width
                height: Math.min(spotifyList.height, root.height * 0.5)
                contentHeight: spotifyList.height
                clip: true
                Column {
                    id: spotifyList
                    width: parent.width
                    Repeater {
                        model: root.themed ? root.rows : []
                        Rectangle {
                            id: row
                            required property var modelData
                            readonly property bool device: root.view === "devices"
                            readonly property bool active: device && modelData.active === true
                            width: spotifyList.width
                            height: 64
                            radius: 6
                            color: hit.pressed && device ? "#3e3e3e" : "transparent"
                            Glyph {
                                id: mark
                                visible: row.device
                                anchors { left: parent.left; leftMargin: 8; verticalCenter: parent.verticalCenter }
                                name: "device"
                                size: 26
                                color: row.active ? root.green : root.grey
                            }
                            Column {
                                anchors { left: row.device ? mark.right : parent.left; leftMargin: row.device ? 16 : 0; right: parent.right; rightMargin: 8; verticalCenter: parent.verticalCenter }
                                spacing: 2
                                Txt {
                                    width: parent.width
                                    text: row.device ? row.modelData.name : row.modelData.title
                                    color: row.active ? root.green : "white"
                                    font.pixelSize: 20
                                    font.bold: row.device
                                    elide: Text.ElideRight
                                }
                                Txt {
                                    width: parent.width
                                    text: !row.device ? row.modelData.artist : row.active ? "Listening on this device" : row.modelData.kind
                                    color: row.active ? root.green : root.grey
                                    font.pixelSize: 16
                                    elide: Text.ElideRight
                                }
                            }
                            MouseArea {
                                id: hit
                                anchors.fill: parent
                                onClicked: root.pick(row.modelData)
                            }
                        }
                    }
                }
            }
            Item {
                width: parent.width
                height: 64
                Rectangle {
                    anchors { right: parent.right; bottom: parent.bottom }
                    width: 140
                    height: 48
                    radius: 24
                    color: "white"
                    scale: closeHit.pressed ? 0.95 : 1
                    Txt {
                        anchors.centerIn: parent
                        text: "Close"
                        color: "#121212"
                        font.pixelSize: 18
                        font.bold: true
                    }
                    MouseArea {
                        id: closeHit
                        anchors.fill: parent
                        onClicked: root.close()
                    }
                }
            }
        }
    }
}
