// 명령 버튼 위젯(`command` 앱의 `command.run`): 누르면 호스트(PC)에 등록된 명령 1건을 실행한다. 랙 내부에 배치된다.
// `kit/LabeledButton`에서 인디케이터를 제외한 형태(상단 버튼 + 하단 라벨)다. 상태가 없는 단발 트리거이므로 인디케이터가 없다
// (상태형 토글은 `CommandSwitchWidget` 참고).
//
// 대상 PC와 명령 ID는 위젯 설정에 저장된다(`widgets/CommandBase`).
// 덱은 실제 명령 내용을 알지 못하며 ID만 전달해 PC 측 등록 목록에 있는 경우에만 실행된다(`docs/04-host-link.md`).
//   - PC가 미연결이거나 명령이 사라졌거나(`command.run` 권한을 허용하지 않은 경우 포함) 종류가 `run`이 아니면 흐리게 표시되고 눌리지 않는다.
//   - 실행 결과는 UI에 별도로 표시하지 않으며, 실패 사유는 데몬과 호스트 로그에만 기록한다.
import QtQuick
import "../kit"

CommandBase {
    id: root

    kindWanted: "run"

    LabeledButton {
        anchors.fill: parent
        opacity: root.fade
        color: root.face
        label: root.label
        indicator: false
        onTapped: {
            if (root.ready)
                root.action("command.run", { id: root.command.id })
        }
    }
}
