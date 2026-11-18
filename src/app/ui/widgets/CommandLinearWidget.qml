// 연속 노브 위젯(`command` 앱의 `command.linear`): 호스트(PC)에 등록된 연속 범위 값(예: 음량 0~100)을 조절한다.
// 랙 내부에 배치되며 `kit/LabeledKnob`(노브, 라벨, 현재 값)으로 렌더링된다.
//
// 값 범위와 단위는 PC가 정의하며(`command.min`, `max`, `step`), 현재 값도 PC가 보고하므로 PC 쪽에서 변경해도 동기화된다.
// 포인터는 좌하단(최솟값)에서 우하단(최댓값)까지 270도 회전하며, 둘레 눈금은 현재 값까지 점등된다.
//   - 노브를 드래그하면(오른쪽/위쪽이 증가) 값이 변한다. `fullPx`만큼 이동하면 전체 범위를 횡단하며, 값은 `step` 단위로 스냅된다.
// 드래그 중에는 `command.set` 액션을 너무 자주 보내지 않도록 `sendMs` 간격으로 스로틀링하고, 손을 뗄 때 마지막 값을 반드시 전송한다
// (PC 측에서도 밀린 요청 중 마지막 값만 적용한다). 조작 직후에는 로컬 값을 먼저 표시하다가 `holdMs` 후 다시 PC 보고 값을 따른다
// (`widgets/CommandBase`의 `hold`).
// PC가 미연결이거나 해당 명령을 제공하지 않으면 흐리게 표시되고 비활성화된다.
import QtQuick
import "../kit"

CommandBase {
    id: root

    kindWanted: "linear"
    fits: !!command && (command.max || 0) > (command.min || 0) && (command.step || 0) > 0

    readonly property real min: ready ? command.min || 0 : 0
    readonly property real max: ready ? command.max || 0 : 1
    readonly property real step: ready ? command.step : 1
    readonly property int fullPx: 280
    readonly property int sendMs: 90
    readonly property int tickCount: 11

    property real local: 0
    readonly property bool known: knob.dragging || holding || value !== undefined
    readonly property real shown: knob.dragging || holding ? local : value !== undefined ? Math.max(min, Math.min(max, value)) : min
    readonly property real fraction: (shown - min) / (max - min)

    readonly property int decimals: {
        const text = String(step)
        const dot = text.indexOf(".")
        return dot < 0 ? 0 : Math.min(3, text.length - dot - 1)
    }
    function fit(v) {
        const ticks = Math.round((Math.max(min, Math.min(max, v)) - min) / step)
        return Math.min(max, Number((min + ticks * step).toFixed(6)))
    }

    property real sent: NaN
    function send() {
        if (local === sent)
            return
        sent = local
        set(local)
    }
    Timer {
        id: pace
        interval: root.sendMs
        onTriggered: if (root.local !== root.sent) { root.send(); restart() }
    }

    property real grabbedAt: 0

    LabeledKnob {
        id: knob
        anchors.fill: parent
        opacity: root.fade
        enabled: root.ready
        color: root.face
        label: root.label
        value: !root.ready ? "" : root.known ? root.shown.toFixed(root.decimals) : "--"
        angle: root.ready ? -135 + 270 * root.fraction : -135
        ticks: {
            const out = []
            for (let i = 0; i < root.tickCount; i++) {
                const at = i / (root.tickCount - 1)
                out.push({ angle: -135 + 270 * at, lit: root.ready && root.known && at <= root.fraction + 0.0001 })
            }
            return out
        }
        onGrabbed: {
            root.grabbedAt = root.shown
            root.local = root.shown
            root.sent = root.shown
        }
        onTurned: (travel) => {
            const v = root.fit(root.grabbedAt + travel / root.fullPx * (root.max - root.min))
            if (v === root.local)
                return
            root.local = v
            if (!pace.running) {
                root.send()
                pace.start()
            }
        }
        onReleased: {
            pace.stop()
            root.send()
            root.hold()
        }
    }
}
