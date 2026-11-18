// 복합 위젯 공통 베이스: 라벨과 나사 디테일이 포함된 위젯 프레임 내부에 콘텐츠를 배치한다.
// 기본 자식 요소는 프레임 내부 콘텐츠 영역에 들어가며, `label`은 데몬이 내려 준 `widget.label`을 기본으로 사용한다(필요 시 오버라이드 가능).
// 헤더 줄 오른쪽 끝에 인디케이터 등을 두려면 `corner`에 지정한다(예: `corner: Indicator { on: ... }`).
//
//   CompositeWidget {
//       Display { anchors.fill: parent; Txt { text: root.sys.host || "" } }
//   }
import QtQuick
import "../kit"

WidgetBase {
    id: root
    default property alias content: frame.content
    property alias corner: frame.corner
    property string label: root.widget.label || ""

    WidgetFrame {
        id: frame
        anchors.fill: parent
        label: root.label
    }
}
