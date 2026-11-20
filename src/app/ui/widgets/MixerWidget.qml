// 음량 믹서 위젯(`mixer` 앱의 `mixer.2x2`, `3x2`, `4x2`, `2x3`, `3x3`, `4x3`, 복합 위젯): 고른 PC에서 소리를 내는 앱마다 스트립 하나를 둔다.
// 6가지 크기는 모양이 같다. 너비에 들어가는 만큼(2칸 3개, 3칸 5개, 4칸 7개) 스트립을 같은 폭으로 놓고, 높이가 늘면 페이더가 길어진다.
//
// 스트립 구성(위에서 아래로):
//   디스플레이   앱 이름, 출력 장치 이름(스트림들이 서로 다른 장치로 나가면 `MIXED`), 음량(%)
//   레벨 미터    디스플레이 안의 LED 눈금(`kit/LevelMeter`). 직전 0.1초의 최대 진폭을 dB로 환산해 초록·노랑·빨강 칸으로 켜고, 피크를 잠시 붙잡아 둔다
//   페이더       위아래로 밀어 음량을 바꾼다(`kit/Fader`, 0~100%). 드래그 중에는 약 0.07초마다 `audio.volume`을 보낸다
//   버튼 두 개   음소거(누르면 `audio.mute`, 음소거 중에는 빨갛게 눌려 있음), 출력 장치 선택(누르면 `shell/AudioSheet` 창을 연다)
// `output_strip` 설정이 켜져 있으면(설정이 없는 위젯도 켜진 것으로 본다) 맨 앞에 기본 출력 장치의 스트립을 두고 구분선을 긋는다.
// 이 스트립의 출력 장치 버튼은 기본 출력 장치를 바꾼다(`audio.default`).
// 스트립이 위젯 너비보다 많으면 옆으로 밀어 넘기고, 맨 아래에 스크롤 막대(홈과 손잡이)를 그려 지금 보는 위치를 알린다.
// 다 들어가면 막대가 없고 옆으로 밀리지 않아 페이지 넘김을 방해하지 않는다.
//
// 데이터는 `shell/WidgetSlot`이 넣어 준다: `audio`(대상 PC의 `{outputs, apps}`, 없으면 `null`), `levels`(`{apps, outputs}`).
// 액션은 대상 PC로 간다. 출력 장치 선택 창은 데몬에 보내지 않는 `audio.pick` 액션으로 연다(`shell/DaemonLink`가 가로챈다).
// 권한(`host.caps`)에 따라: `audio.info`가 없으면 `Not shared`, `audio.control`이 없으면 조작 부품이 흐려지고 눌리지 않는다.
// PC가 연결되어 있지 않거나 오디오 서버에 붙지 못했으면(상태가 `null`) 가운데에 사유를 쓴다.
import QtQuick
import "../kit"

CompositeWidget {
    id: root

    readonly property var caps: host && host.caps ? host.caps : []
    readonly property bool canSee: caps.indexOf("audio.info") >= 0
    readonly property bool canControl: caps.indexOf("audio.control") >= 0
    readonly property var mix: root.audio
    readonly property bool live: !!host && canSee && !!mix
    readonly property bool withOutput: !(widget.settings && widget.settings.output_strip === false)
    readonly property var outputs: mix ? mix.outputs : []
    readonly property var main: {
        for (const o of outputs)
            if (o.default)
                return o
        return null
    }
    readonly property var strips: {
        if (!live)
            return []
        const out = []
        if (withOutput && main)
            out.push({ key: "output:" + main.id, kind: "output", id: main.id, name: "OUTPUT", via: main.title, volume: main.volume, muted: main.muted, output: main.id })
        for (const a of mix.apps)
            out.push({ key: "app:" + a.id, kind: "app", id: a.id, name: a.name, via: titleOf(a.output), volume: a.volume, muted: a.muted, output: a.output })
        return out
    }
    readonly property var byKey: {
        const m = {}
        for (const s of strips)
            m[s.key] = s
        return m
    }
    readonly property string keys: strips.map(s => s.key).join("\n")
    readonly property var blank: ({ key: "", kind: "app", id: "", name: "", via: "", volume: 0, muted: false, output: "" })
    function titleOf(id) {
        if (id === "")
            return "MIXED"
        for (const o of outputs)
            if (o.id === id)
                return o.title
        return ""
    }
    function levelOf(strip) {
        const of = strip.kind === "output" ? root.levels.outputs : root.levels.apps
        const v = of ? of[strip.id] : undefined
        return v > 0 ? v : 0
    }
    function target(strip) {
        return strip.kind === "output" ? { output: strip.id } : { app: strip.id }
    }

    label: (widget.label || "MIXER") + (host ? " · " + host.name.toUpperCase() : "")
    corner: Indicator {
        on: root.live
        color: Theme.indicatorGreen
    }

    readonly property int gap: 10
    readonly property int minStrip: 84
    readonly property int fit: Math.max(1, Math.floor((width - 2 * Theme.framePad + gap) / (minStrip + gap)))

    Display {
        anchors.fill: parent
        visible: !root.live || root.strips.length === 0
        Txt {
            anchors.centerIn: parent
            width: parent.width - 40
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
            text: !root.host ? "PC offline" : !root.canSee ? "Not shared" : !root.mix ? "No sound server on " + root.host.name : "Nothing playing"
            color: Theme.displayDim
            font.pixelSize: Theme.fBody
        }
    }

    readonly property bool more: root.live && root.strips.length > root.fit

    ListView {
        id: list
        anchors { fill: parent; bottomMargin: root.more ? 14 : 0 }
        visible: root.live && root.strips.length > 0
        orientation: ListView.Horizontal
        spacing: root.gap
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        interactive: root.more
        model: root.keys === "" ? [] : root.keys.split("\n")
        readonly property real stripW: Math.max(1, (width - (root.fit - 1) * root.gap) / root.fit)
        onStripWChanged: positionViewAtBeginning()
        delegate: Strip {
            required property string modelData
            required property int index
            width: list.stripW
            height: list.height
            strip: root.byKey[modelData] || root.blank
            divided: modelData.startsWith("output:") && index === 0 && list.count > 1
        }
    }

    Rectangle {
        id: track
        visible: root.more
        anchors { left: parent.left; right: parent.right; bottom: parent.bottom; leftMargin: 12; rightMargin: 12; bottomMargin: 1 }
        height: 6
        radius: 3
        gradient: Gradient {
            GradientStop { position: 0; color: Theme.edgeLo }
            GradientStop { position: 1; color: Theme.edgeHi }
        }
        Rectangle {
            anchors { fill: parent; margins: 1 }
            radius: 2
            color: Theme.well
        }
        Rectangle {
            readonly property real span: Math.max(1, list.contentWidth - list.width)
            width: Math.max(24, track.width * list.width / Math.max(list.width, list.contentWidth))
            height: parent.height - 2
            y: 1
            x: 1 + (track.width - 2 - width) * Math.max(0, Math.min(1, (list.contentX - list.originX) / span))
            radius: 2
            color: Theme.buttonColor("bone")
        }
    }

    component Strip: Item {
        id: s
        property var strip: root.blank
        property bool divided: false
        readonly property real buttonH: 40
        property real pending: -1

        Timer {
            id: throttle
            interval: 70
            onTriggered: {
                if (s.pending < 0)
                    return
                root.action("audio.volume", Object.assign(root.target(s.strip), { volume: s.pending }))
                s.pending = -1
            }
        }
        function sendVolume(v) {
            pending = Math.round(v * 100) / 100
            if (!throttle.running) {
                throttle.triggered()
                throttle.start()
            }
        }

        Display {
            id: head
            width: parent.width
            height: 58
            Column {
                anchors { fill: parent; leftMargin: 7; rightMargin: 5; topMargin: 3 }
                spacing: 0
                Txt {
                    width: parent.width
                    text: s.strip.name
                    color: Theme.displayInk
                    font.pixelSize: 14
                    font.bold: true
                    elide: Text.ElideRight
                }
                Txt {
                    width: parent.width
                    text: s.strip.via
                    color: Theme.displayDim
                    font.pixelSize: 10
                    elide: Text.ElideRight
                }
                Txt {
                    width: parent.width
                    text: s.strip.muted ? "MUTED" : Math.round((fader.dragging || fader.holding ? fader.dragValue : s.strip.volume) * 100) + "%"
                    color: s.strip.muted ? Theme.indicatorRed : Theme.displayInk
                    font.family: Theme.mono
                    font.pixelSize: 13
                }
            }
        }

        Item {
            id: middle
            anchors { top: head.bottom; topMargin: 6; bottom: buttons.top; bottomMargin: 6; left: parent.left; right: parent.right }
            Display {
                id: meterBox
                width: 20
                height: parent.height
                LevelMeter {
                    anchors { fill: parent; margins: 4 }
                    level: root.levelOf(s.strip)
                    dimmed: s.strip.muted
                }
            }
            Fader {
                id: fader
                anchors { left: meterBox.right; leftMargin: 6; right: parent.right }
                height: parent.height
                value: Math.min(1, s.strip.volume)
                interactive: root.canControl
                opacity: root.canControl ? 1 : Theme.dimmed
                color: s.strip.muted ? Theme.indicatorRed : Theme.indicatorAmber
                onMoved: (v) => s.sendVolume(v)
                onReleased: (v) => {
                    throttle.stop()
                    s.pending = -1
                    root.action("audio.volume", Object.assign(root.target(s.strip), { volume: Math.round(v * 100) / 100 }))
                }
            }
        }

        Row {
            id: buttons
            anchors.bottom: parent.bottom
            width: parent.width
            spacing: 4
            Button {
                id: muteButton
                width: (buttons.width - buttons.spacing) / 2
                height: s.buttonH
                interactive: root.canControl
                opacity: root.canControl ? 1 : Theme.dimmed
                held: s.strip.muted
                color: s.strip.muted ? Theme.buttonColor("red") : Theme.buttonColor("bone")
                onTapped: root.action("audio.mute", Object.assign(root.target(s.strip), { on: !s.strip.muted }))
                Glyph {
                    anchors.centerIn: parent
                    name: s.strip.muted ? "mute" : "speaker"
                    size: 20
                    color: muteButton.inkColor
                }
            }
            Button {
                id: outButton
                width: (buttons.width - buttons.spacing) / 2
                height: s.buttonH
                interactive: root.canControl && root.outputs.length > 1
                opacity: interactive ? 1 : Theme.dimmed
                onTapped: root.action("audio.pick", { app: s.strip.kind === "app" ? s.strip.id : "", title: s.strip.name, output: s.strip.output })
                Glyph {
                    anchors.centerIn: parent
                    name: "device"
                    size: 20
                    color: outButton.inkColor
                }
            }
        }

        Rectangle {
            visible: s.divided
            x: s.width + root.gap / 2
            anchors { top: parent.top; bottom: parent.bottom; topMargin: 4; bottomMargin: 4 }
            width: 1
            color: Theme.mix(Theme.plate, Theme.edgeLo, 0.8)
        }
    }
}
