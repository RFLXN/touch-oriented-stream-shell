// 명령 위젯 공통 베이스: `command` 앱의 4개 위젯(`run`, `switch`, `stepped`, `linear`)이 공유하는 속성과 동작을 제공한다.
// 시각 요소는 없으며 실제 UI(`kit/LabeledButton`, `kit/LabeledKnob`)는 각 하위 위젯에서 배치한다.
//
// 대상 PC와 명령 ID는 위젯 설정(`widget.settings.host`, `widget.settings.command`, `WidgetBase`의 `command`)에서 가져온다.
//   kindWanted   이 위젯이 처리할 명령 종류(`run` / `switch` / `stepped` / `linear`)
//   fits         명령 메타데이터가 이 위젯에 맞는지 여부(예: 2개 이상의 단계, 유효한 범위). 하위 위젯에서 정의한다.
//   ready        PC가 연결되어 있고 해당 명령이 존재하며 종류/형식이 일치하는지 여부. `false`면 흐리게(`fade`) 비활성화된다.
//   face, label  버튼/노브 색상과 라벨(위젯 설정 기준)
// 상태형 명령(스위치, 노브)은 `set` 요청 후 응답을 기다리는 동안 낙관적 UI 상태를 유지한다:
//   hold()       `holdMs` 동안 로컬 선택 값을 우선 표시(`holding = true`). 만료 후 다시 PC 보고 값을 따른다.
//   refused()    방금 보낸 액션이 거절되었을 때 발생(`holding`은 이미 해제된 상태).
// 명령이 사라지거나 `ready`가 바뀌면 `holding`을 즉시 해제한다.
import QtQuick
import "../kit"

WidgetBase {
    id: root

    property string kindWanted: ""
    property bool fits: true
    readonly property bool ready: !!command && (command.kind || "run") === kindWanted && fits

    readonly property color face: Theme.buttonColor(widget.color || "bone")
    readonly property string label: widget.label || ""
    readonly property real fade: ready ? 1 : Theme.dimmed

    readonly property int holdMs: Theme.tHold
    property bool holding: false
    signal refused()

    function hold() {
        holding = true
        holdTimer.restart()
    }
    function set(v) { action("command.set", { id: command.id, value: v }) }

    Timer { id: holdTimer; interval: root.holdMs; onTriggered: root.holding = false }
    onReadyChanged: holding = false
    onResultChanged: {
        if (!result || result.ok || Date.now() - result.at > holdMs)
            return
        holding = false
        refused()
    }
}
