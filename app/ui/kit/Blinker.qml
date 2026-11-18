// 공통 점멸 타이머: `active`가 true인 동안 `phase`가 주기적으로 반전되며, 비활성화되면 `true`로 복귀한다.
// 모든 `Indicator`와 `IconIndicator`가 같은 위상으로 깜빡이도록 공용 컴포넌트 하나로 묶었다(시각 요소 없음).
import QtQuick

Timer {
    id: root
    property bool active: false
    property bool phase: true

    interval: 450
    repeat: true
    running: active
    onTriggered: phase = !phase
    onRunningChanged: if (!running) phase = true
}
