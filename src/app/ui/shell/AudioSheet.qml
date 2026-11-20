// 출력 장치 선택 창: 음량 믹서(`widgets/MixerWidget`)의 출력 장치 버튼을 누르면 화면 가운데에 그 PC의 출력 장치 목록을 띄운다.
// 무엇을 고르는지는 `link.audioPick`이 정한다(`shell/DaemonLink`):
//   앱의 스트립       고른 장치로 그 앱의 소리를 옮긴다(`audio.route`)
//   출력 장치 스트립  고른 장치를 PC의 기본 출력 장치로 정한다(`audio.default`)
// 지금 장치는 인디케이터로 표시하고, 누르면 아무것도 하지 않고 닫는다. 바깥 영역이나 닫기 버튼을 누르면 닫힌다.
// 모양은 설정 화면과 같은 부품(라벨, `SettingsRow`, 인디케이터, `CLOSE` 버튼)을 쓴다(`SpotifySheet`의 덱 테마와 같다).
import QtQuick
import "../kit"

Dialog {
    id: root
    property var link: null
    readonly property var pick: link && link.connected ? link.audioPick : null
    readonly property var mix: pick && link.audio ? link.audio[pick.host] || null : null
    readonly property var rows: mix ? mix.outputs : []
    readonly property bool forApp: !!pick && pick.app !== ""
    readonly property string current: {
        if (!pick)
            return ""
        if (forApp)
            return pick.output
        for (const o of rows)
            if (o.default)
                return o.id
        return ""
    }

    function close() { link.audioPick = null }
    function choose(row) {
        if (row.id !== current) {
            if (forApp)
                link.action("audio.route", { app: pick.app, output: row.id }, pick.host, pick.widget)
            else
                link.action("audio.default", { output: row.id }, pick.host, pick.widget)
        }
        close()
    }
    visible: !!pick
    onDismissed: close()
    Dialog.Card {
        anchors.centerIn: parent
        width: parent.width - 2 * Theme.gridMargin - 80
        height: body.height + 44
        Column {
            id: body
            x: 22
            y: 22
            width: parent.width - 44
            spacing: 14
            Label {
                width: parent.width
                text: root.forApp ? "PLAY " + (root.pick ? root.pick.title.toUpperCase() : "") + " ON" : "DEFAULT OUTPUT"
                elide: Text.ElideRight
            }
            Txt {
                visible: root.rows.length === 0
                width: parent.width
                text: "No output device found."
                wrapMode: Text.WordWrap
                color: Theme.inkDim
                font.pixelSize: Theme.fBody
            }
            Flickable {
                visible: root.rows.length > 0
                width: parent.width
                height: Math.min(list.height, root.height * 0.55)
                contentHeight: list.height
                boundsBehavior: Flickable.StopAtBounds
                clip: true
                Column {
                    id: list
                    width: parent.width
                    Repeater {
                        model: root.rows
                        SettingsRow {
                            required property var modelData
                            required property int index
                            width: list.width
                            title: modelData.title
                            note: modelData.default ? "Default" : ""
                            divider: index < root.rows.length - 1
                            onTapped: root.choose(modelData)
                            Indicator {
                                anchors.verticalCenter: parent.verticalCenter
                                on: modelData.id === root.current
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
    }
}
