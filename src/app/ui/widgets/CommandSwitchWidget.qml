// 명령 스위치 위젯(`command` 앱의 `command.switch`): 호스트(PC)에 등록된 스위치를 켜거나 끈다. 랙 내부에 배치된다.
// `kit/LabeledButton`의 토글 형태로 렌더링되며, 켜진 상태에서는 버튼이 눌린 상태를 유지하고 인디케이터가 점등된다.
//
// ON/OFF 상태는 PC가 보고하므로(`value === 1`이면 켜짐) PC 쪽에서 직접 변경해도 UI에 반영된다.
// 탭하면 반대 값으로 `command.set` 액션을 보낸다. 응답이 오기 전에도 즉각적인 반응감을 주기 위해 낙관적으로 상태를 먼저 바꾸고
// `holdMs` 후 다시 PC 보고 값을 따른다. 그사이에 PC가 새 값을 보내면 유지되고, 바뀌지 않으면(명령 실패) 원래 상태로 복귀한다
// (이 홀드 타이머와 거절 감지는 `widgets/CommandBase`가 처리한다).
//   - PC가 미연결이거나 해당 스위치를 제공하지 않으면 흐리게 표시되고 눌리지 않는다.
//   - 액션이 거절되면 인디케이터가 빨간색으로 한 번 점멸한다.
// 인디케이터 색상은 위젯 설정 `light`(`Theme.indicatorColors`의 이름)를 따르고, 비어 있으면 버튼 색상에 맞춘다(`Theme.indicatorFor`).
import QtQuick
import "../kit"

CommandBase {
    id: root

    kindWanted: "switch"

    property bool local: false
    readonly property bool on: holding ? local : value === 1

    property bool rejected: false
    Timer { id: rejectedFor; interval: 400; onTriggered: root.rejected = false }
    onRefused: {
        rejected = true
        rejectedFor.restart()
        button.flash()
    }

    LabeledButton {
        id: button
        anchors.fill: parent
        opacity: root.fade
        color: root.face
        label: root.label
        on: root.ready && root.on
        indicatorColor: root.rejected ? Theme.indicatorRed : Theme.indicatorNamed(root.widget.settings ? root.widget.settings.light : "", root.widget.color)
        onTapped: {
            if (!root.ready)
                return
            root.local = !root.on
            root.hold()
            root.set(root.local ? 1 : 0)
        }
    }
}
