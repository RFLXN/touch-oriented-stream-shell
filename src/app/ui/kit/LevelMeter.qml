// 레벨 미터: 디스플레이 안에 세로로 쌓은 LED 눈금. 음량 믹서가 스트립마다 하나씩 쓴다.
//
// `level`은 진폭(0~1)이며 -60~0 dB로 환산해 아래부터 칸을 켠다. 색은 하드웨어 미터처럼 구간으로 나눈다:
//   초록 -18 dB 아래, 노랑 -18~-6 dB, 빨강 -6 dB 위
// 꺼진 칸도 자기 색을 아주 옅게 띤다(꺼진 LED 렌즈). 켜진 칸은 가운데가 조금 밝다.
// 움직임은 미터의 탄도를 흉내 낸다: 오를 때는 바로 따라가고, 내릴 때는 초당 `fall` dB씩 떨어진다.
// 가장 높이 올라간 칸 하나는 `hold` 동안 켜 둔 뒤 천천히 내려온다(피크 홀드). `dimmed`면 전체를 흐리게 그린다(음소거).
import QtQuick

Item {
    id: root
    property real level: 0
    property bool dimmed: false
    property real fall: 24
    property int hold: 1200

    readonly property real floorDb: -60
    readonly property real target: level > 0 ? Math.max(0, Math.min(1, (20 * Math.log(level) / Math.LN10 - floorDb) / -floorDb)) : 0
    property real shown: 0
    property real peak: 0
    property real peakAt: 0

    readonly property int gap: 2
    readonly property int count: Math.max(6, Math.floor((height + gap) / 7))
    readonly property real segH: (height - (count - 1) * gap) / count
    readonly property int lit: Math.round(shown * count)
    readonly property int peakSeg: Math.round(peak * count) - 1

    readonly property color green: "#35d46a"
    readonly property color yellow: "#ffc22e"
    readonly property color red: "#ff4433"
    function zone(i) {
        const db = floorDb * (1 - (i + 1) / count)
        return db > -6 ? red : db > -18 ? yellow : green
    }

    onTargetChanged: {
        if (target >= shown)
            shown = target
        if (target >= peak) {
            peak = target
            peakAt = Date.now()
        }
    }
    Timer {
        interval: 33
        repeat: true
        running: root.visible && (root.shown > root.target || root.peak > 0)
        onTriggered: {
            const step = root.fall / -root.floorDb * interval / 1000
            root.shown = Math.max(root.target, root.shown - step)
            if (Date.now() - root.peakAt > root.hold)
                root.peak = Math.max(root.shown, root.peak - step * 0.6, 0)
            if (root.peak < 0.5 / root.count)
                root.peak = 0
        }
    }

    opacity: dimmed ? 0.45 : 1
    Repeater {
        model: root.count
        Rectangle {
            id: led
            required property int index
            readonly property color hue: root.zone(index)
            readonly property bool on: index < root.lit || (index === root.peakSeg && root.peak > root.shown)
            width: root.width
            height: root.segH
            y: root.height - (index + 1) * root.segH - index * root.gap
            radius: 1
            color: on ? hue : Theme.mix(Theme.display, hue, Theme.dark ? 0.16 : 0.22)
            Rectangle {
                visible: led.on
                anchors { fill: parent; leftMargin: 2; rightMargin: 2; topMargin: 1; bottomMargin: 1 }
                radius: 1
                color: Qt.lighter(led.hue, 1.25)
                opacity: 0.55
            }
        }
    }
}
