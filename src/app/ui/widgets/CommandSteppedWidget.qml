// 단계 노브 위젯(`command` 앱의 `command.stepped`): 호스트(PC)에 등록된 단계 목록 중 하나를 선택한다
// (예: `off` / `passive` / `active` / `auto`). 랙 내부에 배치되며 `kit/LabeledKnob`(노브, 라벨, 현재 단계명)으로 렌더링된다.
//
// 단계 이름과 개수는 PC가 정의하며(`command.steps`), 현재 단계 인덱스(0부터 시작)도 PC가 보고한다.
// 노브 둘레에 단계 수만큼 눈금이 배치되고 포인터가 현재 단계 눈금을 가리킨다.
//   - 노브를 드래그하면(오른쪽/위쪽이 다음 단계) `stepPx`마다 한 단계씩 전환되며 양 끝에서 멈춘다.
//   - 노브를 탭하면 다음 단계로 순환한다(마지막 단계에서는 처음으로 복귀).
// 단계가 바뀔 때마다 `command.set` 액션을 보낸다. 선택한 값을 먼저 표시하고 잠시(`holdMs`) 뒤부터 다시 PC 보고 값을 따른다
// (`widgets/CommandBase`의 `hold`).
// 그사이에 PC가 단계 개수를 줄이면 선택된 인덱스를 마지막 유효 단계로 클램프한다.
// PC가 미연결이거나 해당 명령을 제공하지 않으면 흐리게 표시되고 비활성화된다.
import QtQuick
import "../kit"

CommandBase {
    id: root

    kindWanted: "stepped"
    fits: !!command && !!command.steps && command.steps.length > 1

    readonly property var steps: ready ? command.steps : []
    readonly property int stepPx: 36
    property int local: 0
    readonly property int index: !ready ? -1
        : knob.dragging || holding ? Math.max(0, Math.min(steps.length - 1, local))
        : value !== undefined && value >= 0 && value < steps.length ? value : -1

    readonly property real spread: steps.length > 1 ? Math.min(45, 270 / (steps.length - 1)) : 0
    function angleOf(i) { return (i - (steps.length - 1) / 2) * spread }

    function choose(i) {
        if (i === root.index && (knob.dragging || holding))
            return
        local = i
        hold()
        set(i)
    }

    property int grabbedAt: 0

    LabeledKnob {
        id: knob
        anchors.fill: parent
        opacity: root.fade
        enabled: root.ready
        color: root.face
        label: root.label
        value: root.index >= 0 ? root.steps[root.index].toUpperCase() : root.ready ? "--" : ""
        angle: root.ready ? root.angleOf(Math.max(0, root.index)) : 0
        ticks: root.steps.map((s, i) => ({ angle: root.angleOf(i), lit: i === root.index }))
        onGrabbed: {
            root.grabbedAt = Math.max(0, root.index)
            root.local = root.grabbedAt
        }
        onTurned: (travel) => {
            const i = Math.max(0, Math.min(root.steps.length - 1, root.grabbedAt + Math.round(travel / root.stepPx)))
            if (i !== root.local)
                root.choose(i)
        }
        onReleased: root.hold()
        onTapped: root.choose((Math.max(-1, root.index) + 1) % root.steps.length)
    }
}
